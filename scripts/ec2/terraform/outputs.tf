output "vpc_id" {
  value = local.vpc_id
}

output "subnet_id" {
  value = local.subnet_id
}

output "instance_id" {
  value = aws_instance.this.id
}

output "volume_id" {
  value = aws_ebs_volume.this.id
}

output "asg_name" {
  value = aws_autoscaling_group.this.name
}

output "lb_arn" {
  value = aws_lb.this.arn
}
